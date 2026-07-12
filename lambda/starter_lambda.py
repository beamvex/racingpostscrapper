import json
import os
import boto3

ecs = boto3.client('ecs')

def lambda_handler(event, context):
    # Extract race URL from the event payload
    race_url = event.get('race_url')
    
    # Get configuration from environment variables
    cluster_arn = os.environ['ECS_CLUSTER_ARN']
    task_def_arn = os.environ['ECS_TASKDEF_PIPELINE_ARN']
    subnets = os.environ['ECS_SUBNETS'].split(',')
    security_groups = os.environ['ECS_SECURITY_GROUPS'].split(',')
    
    # Build ECS task parameters
    ecs_params = {
        'cluster': cluster_arn,
        'taskDefinition': task_def_arn,
        'launchType': 'FARGATE',
        'networkConfiguration': {
            'awsvpcConfiguration': {
                'subnets': subnets,
                'securityGroups': security_groups,
                'assignPublicIp': 'ENABLED'
            }
        }
    }
    
    # Add container override for race URL if provided
    if race_url:
        ecs_params['overrides'] = {
            'containerOverrides': [
                {
                    'name': 'daily-pipeline',
                    'environment': [
                        {
                            'name': 'RACE_URL',
                            'value': race_url
                        }
                    ]
                }
            ]
        }
    
    # Run the ECS task
    response = ecs.run_task(**ecs_params)
    
    return {
        'statusCode': 200,
        'body': json.dumps({
            'message': 'ECS task started',
            'taskArn': response['tasks'][0]['taskArn'] if response['tasks'] else None,
            'raceUrl': race_url
        })
    }
